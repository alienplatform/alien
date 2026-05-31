# CreateManagerResponseNetworkByoVnetAzure1

## Example Usage

```typescript
import { CreateManagerResponseNetworkByoVnetAzure1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseNetworkByoVnetAzure1 = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `privateSubnetName`                                                                                  | *string*                                                                                             | :heavy_check_mark:                                                                                   | Name of the private subnet within the VNet                                                           |
| `publicSubnetName`                                                                                   | *string*                                                                                             | :heavy_check_mark:                                                                                   | Name of the public subnet within the VNet                                                            |
| `type`                                                                                               | [models.CreateManagerResponseTypeByoVnetAzure1](../models/createmanagerresponsetypebyovnetazure1.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `vnetResourceId`                                                                                     | *string*                                                                                             | :heavy_check_mark:                                                                                   | The full resource ID of the existing VNet                                                            |