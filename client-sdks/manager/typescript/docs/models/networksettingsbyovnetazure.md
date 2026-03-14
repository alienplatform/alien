# NetworkSettingsByoVnetAzure

Use an existing VNet (Azure).

Alien validates the references but creates no networking infrastructure.
The customer is responsible for routing and egress (NAT Gateway, proxy, VPN, etc.).

## Example Usage

```typescript
import { NetworkSettingsByoVnetAzure } from "@alienplatform/manager-api/models";

let value: NetworkSettingsByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `privateSubnetName`                        | *string*                                   | :heavy_check_mark:                         | Name of the private subnet within the VNet |
| `publicSubnetName`                         | *string*                                   | :heavy_check_mark:                         | Name of the public subnet within the VNet  |
| `type`                                     | *"byo-vnet-azure"*                         | :heavy_check_mark:                         | N/A                                        |
| `vnetResourceId`                           | *string*                                   | :heavy_check_mark:                         | The full resource ID of the existing VNet  |