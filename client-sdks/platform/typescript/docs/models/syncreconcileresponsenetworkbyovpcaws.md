# SyncReconcileResponseNetworkByoVpcAws

## Example Usage

```typescript
import { SyncReconcileResponseNetworkByoVpcAws } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `privateSubnetIds`                                                                           | *string*[]                                                                                   | :heavy_check_mark:                                                                           | IDs of private subnets                                                                       |
| `publicSubnetIds`                                                                            | *string*[]                                                                                   | :heavy_check_mark:                                                                           | IDs of public subnets (required for public ingress)                                          |
| `securityGroupIds`                                                                           | *string*[]                                                                                   | :heavy_minus_sign:                                                                           | Optional security group IDs to use                                                           |
| `type`                                                                                       | [models.SyncReconcileResponseTypeByoVpcAws](../models/syncreconcileresponsetypebyovpcaws.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `vpcId`                                                                                      | *string*                                                                                     | :heavy_check_mark:                                                                           | The ID of the existing VPC                                                                   |