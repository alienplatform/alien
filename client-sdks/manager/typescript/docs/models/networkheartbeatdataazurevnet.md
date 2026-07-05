# NetworkHeartbeatDataAzureVnet

## Example Usage

```typescript
import { NetworkHeartbeatDataAzureVnet } from "@alienplatform/manager-api/models";

let value: NetworkHeartbeatDataAzureVnet = {
  isByoVnet: true,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "azureVnet",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `applicationGatewaySubnetName`                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `cidrBlock`                                                          | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `isByoVnet`                                                          | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `lastByoVnetVerificationErrorCode`                                   | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `location`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `natGatewayId`                                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `nsgId`                                                              | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `privateEndpointSubnetName`                                          | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `privateSubnetName`                                                  | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `publicIpId`                                                         | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `publicSubnetName`                                                   | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `resourceGroup`                                                      | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [models.NetworkHeartbeatStatus](../models/networkheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `vnetName`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `vnetResourceId`                                                     | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `backend`                                                            | *"azureVnet"*                                                        | :heavy_check_mark:                                                   | N/A                                                                  |