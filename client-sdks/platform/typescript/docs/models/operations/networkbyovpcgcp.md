# NetworkByoVpcGcp

## Example Usage

```typescript
import { NetworkByoVpcGcp } from "@alienplatform/platform-api/models/operations";

let value: NetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `networkName`                                                        | *string*                                                             | :heavy_check_mark:                                                   | The name of the existing VPC network                                 |
| `region`                                                             | *string*                                                             | :heavy_check_mark:                                                   | The region of the subnet                                             |
| `subnetName`                                                         | *string*                                                             | :heavy_check_mark:                                                   | The name of the subnet to use                                        |
| `type`                                                               | [operations.TypeByoVpcGcp](../../models/operations/typebyovpcgcp.md) | :heavy_check_mark:                                                   | N/A                                                                  |