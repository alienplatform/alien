# CreateManagerResponseNetworkByoVpcAws2

## Example Usage

```typescript
import { CreateManagerResponseNetworkByoVpcAws2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseNetworkByoVpcAws2 = {
  privateSubnetIds: [],
  publicSubnetIds: [],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `privateSubnetIds`                                                                             | *string*[]                                                                                     | :heavy_check_mark:                                                                             | IDs of private subnets                                                                         |
| `publicSubnetIds`                                                                              | *string*[]                                                                                     | :heavy_check_mark:                                                                             | IDs of public subnets (required for public ingress)                                            |
| `securityGroupIds`                                                                             | *string*[]                                                                                     | :heavy_minus_sign:                                                                             | Optional security group IDs to use                                                             |
| `type`                                                                                         | [models.CreateManagerResponseTypeByoVpcAws2](../models/createmanagerresponsetypebyovpcaws2.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `vpcId`                                                                                        | *string*                                                                                       | :heavy_check_mark:                                                                             | The ID of the existing VPC                                                                     |