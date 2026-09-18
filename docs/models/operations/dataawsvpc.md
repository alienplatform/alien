# DataAwsVpc

## Example Usage

```typescript
import { DataAwsVpc } from "@alienplatform/platform-api/models/operations";

let value: DataAwsVpc = {
  availabilityZones: [
    "<value 1>",
    "<value 2>",
  ],
  isByoVpc: true,
  privateSubnetIds: [],
  publicSubnetIds: [],
  routeTableCount: 759318,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "awsVpc",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `availabilityZones`                                                | *string*[]                                                         | :heavy_check_mark:                                                 | N/A                                                                |
| `cidrBlock`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `internetGatewayId`                                                | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `isByoVpc`                                                         | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `natGatewayId`                                                     | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `privateSubnetIds`                                                 | *string*[]                                                         | :heavy_check_mark:                                                 | N/A                                                                |
| `publicSubnetIds`                                                  | *string*[]                                                         | :heavy_check_mark:                                                 | N/A                                                                |
| `routeTableCount`                                                  | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `securityGroupId`                                                  | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus46](../../models/operations/datastatus46.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `vpcId`                                                            | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `vpcState`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `backend`                                                          | *"awsVpc"*                                                         | :heavy_check_mark:                                                 | N/A                                                                |