# NetworkByoVpcAws

## Example Usage

```typescript
import { NetworkByoVpcAws } from "@alienplatform/platform-api/models/operations";

let value: NetworkByoVpcAws = {
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `privateSubnetIds`                                                   | *string*[]                                                           | :heavy_check_mark:                                                   | IDs of private subnets                                               |
| `publicSubnetIds`                                                    | *string*[]                                                           | :heavy_check_mark:                                                   | IDs of public subnets (required for public ingress)                  |
| `securityGroupIds`                                                   | *string*[]                                                           | :heavy_minus_sign:                                                   | Optional security group IDs to use                                   |
| `type`                                                               | [operations.TypeByoVpcAws](../../models/operations/typebyovpcaws.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `vpcId`                                                              | *string*                                                             | :heavy_check_mark:                                                   | The ID of the existing VPC                                           |