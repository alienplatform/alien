# NetworkByoVnetAzure

## Example Usage

```typescript
import { NetworkByoVnetAzure } from "@alienplatform/platform-api/models/operations";

let value: NetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `privateSubnetName`                                                        | *string*                                                                   | :heavy_check_mark:                                                         | Name of the private subnet within the VNet                                 |
| `publicSubnetName`                                                         | *string*                                                                   | :heavy_check_mark:                                                         | Name of the public subnet within the VNet                                  |
| `type`                                                                     | [operations.TypeByoVnetAzure](../../models/operations/typebyovnetazure.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `vnetResourceId`                                                           | *string*                                                                   | :heavy_check_mark:                                                         | The full resource ID of the existing VNet                                  |