# DeploymentNetworkByoVnetAzure

## Example Usage

```typescript
import { DeploymentNetworkByoVnetAzure } from "@aliendotdev/platform-api/models";

let value: DeploymentNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `privateSubnetName`                                                          | *string*                                                                     | :heavy_check_mark:                                                           | Name of the private subnet within the VNet                                   |
| `publicSubnetName`                                                           | *string*                                                                     | :heavy_check_mark:                                                           | Name of the public subnet within the VNet                                    |
| `type`                                                                       | [models.DeploymentTypeByoVnetAzure](../models/deploymenttypebyovnetazure.md) | :heavy_check_mark:                                                           | N/A                                                                          |
| `vnetResourceId`                                                             | *string*                                                                     | :heavy_check_mark:                                                           | The full resource ID of the existing VNet                                    |