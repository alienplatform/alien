# DataAwsVpc

## Example Usage

```typescript
import { DataAwsVpc } from "@alienplatform/platform-api/models";

let value: DataAwsVpc = {
  availabilityZones: [
    "<value 1>",
    "<value 2>",
  ],
  events: [],
  isByoVpc: true,
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  routeTableCount: 642691,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: true,
  },
  backend: "awsVpc",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `availabilityZones`                                                              | *string*[]                                                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `cidrBlock`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent40](../models/syncreconcilerequestevent40.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `internetGatewayId`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `isByoVpc`                                                                       | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `natGatewayId`                                                                   | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `privateSubnetIds`                                                               | *string*[]                                                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `publicSubnetIds`                                                                | *string*[]                                                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `routeTableCount`                                                                | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `securityGroupId`                                                                | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus40](../models/heartbeatstatus40.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `vpcId`                                                                          | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `vpcState`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `backend`                                                                        | *"awsVpc"*                                                                       | :heavy_check_mark:                                                               | N/A                                                                              |