# PrepareDeploymentStackNetworkByoVnetAzure

## Example Usage

```typescript
import { PrepareDeploymentStackNetworkByoVnetAzure } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `applicationGatewaySubnetName`                                                                                         | *string*                                                                                                               | :heavy_minus_sign:                                                                                                     | Name of the dedicated classic Application Gateway subnet within the VNet.                                              |
| `privateSubnetName`                                                                                                    | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Name of the private subnet within the VNet                                                                             |
| `publicSubnetName`                                                                                                     | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Name of the public subnet within the VNet                                                                              |
| `type`                                                                                                                 | [operations.PrepareDeploymentStackTypeByoVnetAzure](../../models/operations/preparedeploymentstacktypebyovnetazure.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `vnetResourceId`                                                                                                       | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | The full resource ID of the existing VNet                                                                              |