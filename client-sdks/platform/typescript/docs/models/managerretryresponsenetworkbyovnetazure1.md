# ManagerRetryResponseNetworkByoVnetAzure1

## Example Usage

```typescript
import { ManagerRetryResponseNetworkByoVnetAzure1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseNetworkByoVnetAzure1 = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `applicationGatewaySubnetName`                                                                     | *string*                                                                                           | :heavy_minus_sign:                                                                                 | Name of the dedicated classic Application Gateway subnet within the VNet.                          |
| `privateSubnetName`                                                                                | *string*                                                                                           | :heavy_check_mark:                                                                                 | Name of the private subnet within the VNet                                                         |
| `publicSubnetName`                                                                                 | *string*                                                                                           | :heavy_check_mark:                                                                                 | Name of the public subnet within the VNet                                                          |
| `type`                                                                                             | [models.ManagerRetryResponseTypeByoVnetAzure1](../models/managerretryresponsetypebyovnetazure1.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `vnetResourceId`                                                                                   | *string*                                                                                           | :heavy_check_mark:                                                                                 | The full resource ID of the existing VNet                                                          |