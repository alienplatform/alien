# CreateManagerResponseNetworkByoVpcGcp3

## Example Usage

```typescript
import { CreateManagerResponseNetworkByoVpcGcp3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseNetworkByoVpcGcp3 = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `networkName`                                                                                  | *string*                                                                                       | :heavy_check_mark:                                                                             | The name of the existing VPC network                                                           |
| `region`                                                                                       | *string*                                                                                       | :heavy_check_mark:                                                                             | The region of the subnet                                                                       |
| `subnetName`                                                                                   | *string*                                                                                       | :heavy_check_mark:                                                                             | The name of the subnet to use                                                                  |
| `type`                                                                                         | [models.CreateManagerResponseTypeByoVpcGcp3](../models/createmanagerresponsetypebyovpcgcp3.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |