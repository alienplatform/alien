# ManagerRetryResponseNetworkByoVnetAzure2

## Example Usage

```typescript
import { ManagerRetryResponseNetworkByoVnetAzure2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseNetworkByoVnetAzure2 = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `privateSubnetName`                                                                                | *string*                                                                                           | :heavy_check_mark:                                                                                 | Name of the private subnet within the VNet                                                         |
| `publicSubnetName`                                                                                 | *string*                                                                                           | :heavy_check_mark:                                                                                 | Name of the public subnet within the VNet                                                          |
| `type`                                                                                             | [models.ManagerRetryResponseTypeByoVnetAzure2](../models/managerretryresponsetypebyovnetazure2.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `vnetResourceId`                                                                                   | *string*                                                                                           | :heavy_check_mark:                                                                                 | The full resource ID of the existing VNet                                                          |