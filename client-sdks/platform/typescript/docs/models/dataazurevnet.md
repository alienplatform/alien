# DataAzureVnet

## Example Usage

```typescript
import { DataAzureVnet } from "@alienplatform/platform-api/models";

let value: DataAzureVnet = {
  events: [],
  isByoVnet: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "azureVnet",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cidrBlock`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent42](../models/syncreconcilerequestevent42.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `isByoVnet`                                                                      | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `lastByoVnetVerificationErrorCode`                                               | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `location`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `natGatewayId`                                                                   | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `nsgId`                                                                          | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `privateSubnetName`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `publicIpId`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `publicSubnetName`                                                               | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceGroup`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus42](../models/heartbeatstatus42.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `vnetName`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `vnetResourceId`                                                                 | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `backend`                                                                        | *"azureVnet"*                                                                    | :heavy_check_mark:                                                               | N/A                                                                              |