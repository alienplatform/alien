# StackSettingsNetworkByoVpcGcp

## Example Usage

```typescript
import { StackSettingsNetworkByoVpcGcp } from "@alienplatform/platform-api/models";

let value: StackSettingsNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `networkName`                                                                | *string*                                                                     | :heavy_check_mark:                                                           | The name of the existing VPC network                                         |
| `region`                                                                     | *string*                                                                     | :heavy_check_mark:                                                           | The region of the subnet                                                     |
| `subnetName`                                                                 | *string*                                                                     | :heavy_check_mark:                                                           | The name of the subnet to use                                                |
| `type`                                                                       | [models.StackSettingsTypeByoVpcGcp](../models/stacksettingstypebyovpcgcp.md) | :heavy_check_mark:                                                           | N/A                                                                          |