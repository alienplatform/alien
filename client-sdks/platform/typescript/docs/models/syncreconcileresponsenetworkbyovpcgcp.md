# SyncReconcileResponseNetworkByoVpcGcp

## Example Usage

```typescript
import { SyncReconcileResponseNetworkByoVpcGcp } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `networkName`                                                                                | *string*                                                                                     | :heavy_check_mark:                                                                           | The name of the existing VPC network                                                         |
| `region`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | The region of the subnet                                                                     |
| `subnetName`                                                                                 | *string*                                                                                     | :heavy_check_mark:                                                                           | The name of the subnet to use                                                                |
| `type`                                                                                       | [models.SyncReconcileResponseTypeByoVpcGcp](../models/syncreconcileresponsetypebyovpcgcp.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |