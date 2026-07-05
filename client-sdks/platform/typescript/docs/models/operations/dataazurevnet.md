# DataAzureVnet

## Example Usage

```typescript
import { DataAzureVnet } from "@alienplatform/platform-api/models/operations";

let value: DataAzureVnet = {
  isByoVnet: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: true,
  },
  backend: "azureVnet",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `applicationGatewaySubnetName`                                     | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `cidrBlock`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `isByoVnet`                                                        | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `lastByoVnetVerificationErrorCode`                                 | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `location`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `natGatewayId`                                                     | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `nsgId`                                                            | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `privateEndpointSubnetName`                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `privateSubnetName`                                                | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `publicIpId`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `publicSubnetName`                                                 | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `resourceGroup`                                                    | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus46](../../models/operations/datastatus46.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `vnetName`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `vnetResourceId`                                                   | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `backend`                                                          | *"azureVnet"*                                                      | :heavy_check_mark:                                                 | N/A                                                                |