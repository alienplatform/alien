# NetworkSettingsByoVpcGcp

Use an existing VPC (GCP).

Alien validates the references but creates no networking infrastructure.
The customer is responsible for routing and egress (Cloud NAT, proxy, VPN, etc.).

## Example Usage

```typescript
import { NetworkSettingsByoVpcGcp } from "@alienplatform/manager-api/models";

let value: NetworkSettingsByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `networkName`                        | *string*                             | :heavy_check_mark:                   | The name of the existing VPC network |
| `region`                             | *string*                             | :heavy_check_mark:                   | The region of the subnet             |
| `subnetName`                         | *string*                             | :heavy_check_mark:                   | The name of the subnet to use        |
| `type`                               | *"byo-vpc-gcp"*                      | :heavy_check_mark:                   | N/A                                  |