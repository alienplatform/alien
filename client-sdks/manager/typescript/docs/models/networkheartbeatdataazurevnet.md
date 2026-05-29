# NetworkHeartbeatDataAzureVnet

## Example Usage

```typescript
import { NetworkHeartbeatDataAzureVnet } from "@alienplatform/manager-api/models";

let value: NetworkHeartbeatDataAzureVnet = {
  events: [],
  isByoVnet: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "azureVnet",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `cidrBlock`                                                          | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `events`                                                             | [models.HeartbeatEvent](../models/heartbeatevent.md)[]               | :heavy_check_mark:                                                   | N/A                                                                  |
| `isByoVnet`                                                          | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `lastByoVnetVerificationErrorCode`                                   | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `location`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `natGatewayId`                                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `nsgId`                                                              | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `privateSubnetName`                                                  | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `publicIpId`                                                         | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `publicSubnetName`                                                   | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `resourceGroup`                                                      | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [models.NetworkHeartbeatStatus](../models/networkheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `vnetName`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `vnetResourceId`                                                     | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `backend`                                                            | *"azureVnet"*                                                        | :heavy_check_mark:                                                   | N/A                                                                  |