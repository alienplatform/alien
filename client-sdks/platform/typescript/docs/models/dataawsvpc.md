# DataAwsVpc

## Example Usage

```typescript
import { DataAwsVpc } from "@alienplatform/platform-api/models";

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
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "awsVpc",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `availabilityZones`                                                        | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `cidrBlock`                                                                | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `internetGatewayId`                                                        | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `isByoVpc`                                                                 | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `natGatewayId`                                                             | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `privateSubnetIds`                                                         | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `publicSubnetIds`                                                          | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `routeTableCount`                                                          | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `securityGroupId`                                                          | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus40](../models/resourceheartbeatstatus40.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `vpcId`                                                                    | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `vpcState`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `backend`                                                                  | *"awsVpc"*                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |