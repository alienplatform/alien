# CreateManagerResponseNetworkByoVpcGcp1

## Example Usage

```typescript
import { CreateManagerResponseNetworkByoVpcGcp1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseNetworkByoVpcGcp1 = {
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
| `type`                                                                                         | [models.CreateManagerResponseTypeByoVpcGcp1](../models/createmanagerresponsetypebyovpcgcp1.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |