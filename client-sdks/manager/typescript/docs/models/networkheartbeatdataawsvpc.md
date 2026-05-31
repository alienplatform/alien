# NetworkHeartbeatDataAwsVpc

## Example Usage

```typescript
import { NetworkHeartbeatDataAwsVpc } from "@alienplatform/manager-api/models";

let value: NetworkHeartbeatDataAwsVpc = {
  availabilityZones: [],
  isByoVpc: true,
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  routeTableCount: 704849,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "awsVpc",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `availabilityZones`                                                  | *string*[]                                                           | :heavy_check_mark:                                                   | N/A                                                                  |
| `cidrBlock`                                                          | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `internetGatewayId`                                                  | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `isByoVpc`                                                           | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `natGatewayId`                                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `privateSubnetIds`                                                   | *string*[]                                                           | :heavy_check_mark:                                                   | N/A                                                                  |
| `publicSubnetIds`                                                    | *string*[]                                                           | :heavy_check_mark:                                                   | N/A                                                                  |
| `routeTableCount`                                                    | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `securityGroupId`                                                    | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [models.NetworkHeartbeatStatus](../models/networkheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `vpcId`                                                              | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `vpcState`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `backend`                                                            | *"awsVpc"*                                                           | :heavy_check_mark:                                                   | N/A                                                                  |