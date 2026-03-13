# DeploymentNetworkByoVpcAws

## Example Usage

```typescript
import { DeploymentNetworkByoVpcAws } from "@aliendotdev/platform-api/models";

let value: DeploymentNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `privateSubnetIds`                                                     | *string*[]                                                             | :heavy_check_mark:                                                     | IDs of private subnets                                                 |
| `publicSubnetIds`                                                      | *string*[]                                                             | :heavy_check_mark:                                                     | IDs of public subnets (required for public ingress)                    |
| `securityGroupIds`                                                     | *string*[]                                                             | :heavy_minus_sign:                                                     | Optional security group IDs to use                                     |
| `type`                                                                 | [models.DeploymentTypeByoVpcAws](../models/deploymenttypebyovpcaws.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `vpcId`                                                                | *string*                                                               | :heavy_check_mark:                                                     | The ID of the existing VPC                                             |